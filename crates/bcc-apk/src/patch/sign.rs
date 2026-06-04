use anyhow::{Context, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use rasn_pkix::Certificate;
use rsa::pkcs8::{DecodePrivateKey, EncodePublicKey};
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::{Pkcs1v15Sign, RsaPublicKey, RsaPrivateKey};
use sha2::{Digest as _, Sha256};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;
use rayon::prelude::*;
use tracing::{debug, trace};

const APK_SIGNING_BLOCK_MAGIC: &[u8] = b"APK Sig Block 42";
const APK_SIGNING_BLOCK_V2_ID: u32 = 0x7109871a;
const RSA_PKCS1V15_SHA2_256: u32 = 0x0103;
const MAX_CHUNK_SIZE: usize = 1024 * 1024;

pub struct ZipInfo {
    pub central_directory_start: u64,
    pub end_of_central_directory_start: u64,
}

impl ZipInfo {
    pub fn new<ReaderType: Read + Seek>(reader_instance: &mut ReaderType) -> Result<Self> {
        let mut end_of_central_directory_magic_buffer = [0u8; 4];
        let total_file_length = reader_instance.seek(SeekFrom::End(0))?;

        let mut backwards_search_position = total_file_length.saturating_sub(22);
        let mut signature_magic_found = false;

        while backwards_search_position > 0 && backwards_search_position >= total_file_length.saturating_sub(0xFFFF + 22) {
            reader_instance.seek(SeekFrom::Start(backwards_search_position))?;
            reader_instance.read_exact(&mut end_of_central_directory_magic_buffer)?;
            if end_of_central_directory_magic_buffer == [0x50, 0x4b, 0x05, 0x06] {
                signature_magic_found = true;
                break;
            }
            backwards_search_position -= 1;
        }

        anyhow::ensure!(signature_magic_found, "End of Central Directory (EOCD) not found. Is this a valid ZIP?");

        reader_instance.seek(SeekFrom::Start(backwards_search_position + 16))?;
        let central_directory_start_address = reader_instance.read_u32::<LittleEndian>()? as u64;

        Ok(ZipInfo {
            central_directory_start: central_directory_start_address,
            end_of_central_directory_start: backwards_search_position,
        })
    }
}

pub struct Signer {
    private_key: RsaPrivateKey,
    public_key: RsaPublicKey,
    certificate_der: Certificate,
}

impl Signer {
    pub fn new(pem_string: &str) -> Result<Self> {
        let certificate_start_tag = "-----BEGIN CERTIFICATE-----";
        let certificate_end_tag = "-----END CERTIFICATE-----";

        let certificate_start_index = pem_string.find(certificate_start_tag).context("No BEGIN CERTIFICATE tag found in PEM")?;
        let certificate_end_index = pem_string.find(certificate_end_tag).context("No END CERTIFICATE tag found in PEM")?;

        let isolated_private_key_string = &pem_string[..certificate_start_index].trim();

        let parsed_private_key = RsaPrivateKey::from_pkcs8_pem(isolated_private_key_string)
            .or_else(|_| RsaPrivateKey::from_pkcs1_pem(isolated_private_key_string))
            .context("Failed to parse RSA Private Key from PEM.")?;

        let derived_public_key = parsed_private_key.to_public_key();

        let isolated_base64_certificate = &pem_string[certificate_start_index + certificate_start_tag.len()..certificate_end_index]
            .replace('\n', "")
            .replace('\r', "");

        let raw_der_decoded_bytes = BASE64_STANDARD.decode(isolated_base64_certificate).context("Failed to base64 decode certificate")?;
        let parsed_certificate_der = rasn::der::decode::<Certificate>(&raw_der_decoded_bytes)
            .map_err(|error| anyhow::anyhow!("Failed to parse ASN.1 Certificate: {}", error))?;

        debug!("Derived RSA Private Key and loaded ASN.1 X.509 Certificate successfully");
        Ok(Self {
            private_key: parsed_private_key,
            public_key: derived_public_key,
            certificate_der: parsed_certificate_der,
        })
    }

    pub fn cert(&self) -> &Certificate {
        &self.certificate_der
    }

    pub fn pubkey(&self) -> &RsaPublicKey {
        &self.public_key
    }

    pub fn execute_signature(&self, data_payload: &[u8]) -> Result<Vec<u8>> {
        let computed_digest = Sha256::digest(data_payload);
        let padding_scheme = Pkcs1v15Sign::new::<Sha256>();
        self.private_key.sign(padding_scheme, &computed_digest).context("RSA signing failed")
    }
}

pub fn sign_apk_file(apk_file_path: &Path, pem_file: Option<String>) -> Result<()> {
    let active_pem_string = crate::pem::get_active_pem(pem_file.as_ref());
    let internal_signer_identity = Signer::new(&active_pem_string)?;
    let raw_apk_bytes = std::fs::read(apk_file_path)?;
    let mut memory_reader = Cursor::new(&raw_apk_bytes);
    let signature_block_information = parse_apk_signing_block(&mut memory_reader)?;

    let computed_zip_hash = compute_digest_parallel(
        &raw_apk_bytes,
        signature_block_information.signing_block_start,
        signature_block_information.central_directory_start,
        signature_block_information.end_of_central_directory_start
    )?;

    let mut generated_new_signature_block = vec![];
    let mut memory_writer = Cursor::new(&mut generated_new_signature_block);
    write_apk_signing_block(&mut memory_writer, computed_zip_hash, &internal_signer_identity)?;

    let mut final_output_file = File::create(apk_file_path)?;

    final_output_file.write_all(&raw_apk_bytes[..(signature_block_information.signing_block_start as usize)])?;
    final_output_file.write_all(&generated_new_signature_block)?;
    let new_central_directory_start_offset = final_output_file.stream_position()?;

    final_output_file.write_all(&raw_apk_bytes[(signature_block_information.central_directory_start as usize)..(signature_block_information.end_of_central_directory_start as usize)])?;
    let new_end_of_central_directory_offset = final_output_file.stream_position()?;

    final_output_file.write_all(&raw_apk_bytes[(signature_block_information.end_of_central_directory_start as usize)..])?;

    final_output_file.seek(SeekFrom::Start(new_end_of_central_directory_offset + 16))?;
    final_output_file.write_u32::<LittleEndian>(new_central_directory_start_offset as u32)?;

    trace!("Injected APK Signature Scheme V2 block completely");
    Ok(())
}

fn compute_digest_parallel(
    raw_apk_bytes: &[u8],
    signing_block_start_address: u64,
    central_directory_start_address: u64,
    end_of_central_directory_start_address: u64,
) -> Result<[u8; 32]> {
    let mut final_master_hasher = Sha256::new();

    let pre_signing_block_bytes = &raw_apk_bytes[..signing_block_start_address as usize];
    let central_directory_bytes = &raw_apk_bytes[(central_directory_start_address as usize)..(end_of_central_directory_start_address as usize)];

    let mut end_of_central_directory_buffer = raw_apk_bytes[(end_of_central_directory_start_address as usize)..].to_vec();
    let mut end_of_central_directory_cursor = Cursor::new(&mut end_of_central_directory_buffer);
    end_of_central_directory_cursor.seek(SeekFrom::Start(16))?;
    end_of_central_directory_cursor.write_u32::<LittleEndian>(signing_block_start_address as u32)?;

    let mut composite_data_chunks: Vec<&[u8]> = Vec::new();
    composite_data_chunks.extend(pre_signing_block_bytes.chunks(MAX_CHUNK_SIZE));
    composite_data_chunks.extend(central_directory_bytes.chunks(MAX_CHUNK_SIZE));
    composite_data_chunks.extend(end_of_central_directory_buffer.chunks(MAX_CHUNK_SIZE));

    let parallel_hash_chunks: Vec<[u8; 32]> = composite_data_chunks
        .into_par_iter()
        .map(|data_chunk| {
            let mut localized_chunk_hasher = Sha256::new();
            localized_chunk_hasher.update([0xa5]);
            localized_chunk_hasher.update((data_chunk.len() as u32).to_le_bytes());
            localized_chunk_hasher.update(data_chunk);
            localized_chunk_hasher.finalize().into()
        })
        .collect();

    final_master_hasher.update([0x5a]);
    final_master_hasher.update((parallel_hash_chunks.len() as u32).to_le_bytes());

    for computed_chunk_hash in &parallel_hash_chunks {
        final_master_hasher.update(computed_chunk_hash);
    }

    Ok(final_master_hasher.finalize().into())
}

#[derive(Debug, Default)]
struct DigestData {
    pub algorithm_identifier: u32,
    pub digest_bytes: Vec<u8>,
}

impl DigestData {
    fn new(computed_hash: [u8; 32]) -> Self {
        Self {
            algorithm_identifier: RSA_PKCS1V15_SHA2_256,
            digest_bytes: computed_hash.to_vec(),
        }
    }

    fn calculate_byte_size(&self) -> u32 {
        self.digest_bytes.len() as u32 + 12
    }

    fn write_to_stream(&self, stream_writer: &mut impl Write) -> Result<()> {
        stream_writer.write_u32::<LittleEndian>(self.digest_bytes.len() as u32 + 8)?;
        stream_writer.write_u32::<LittleEndian>(self.algorithm_identifier)?;
        stream_writer.write_u32::<LittleEndian>(self.digest_bytes.len() as u32)?;
        stream_writer.write_all(&self.digest_bytes)?;
        Ok(())
    }
}

#[derive(Debug, Default)]
struct SignedDataBlock {
    pub attached_digests: Vec<DigestData>,
    pub attached_certificates: Vec<Vec<u8>>,
    pub attached_additional_attributes: Vec<(u32, Vec<u8>)>,
}

impl SignedDataBlock {
    fn new(computed_hash: [u8; 32], active_signer: &Signer) -> Result<Self> {
        Ok(Self {
            attached_digests: vec![DigestData::new(computed_hash)],
            attached_certificates: vec![
                rasn::der::encode(active_signer.cert()).map_err(|error| anyhow::anyhow!("{}", error))?
            ],
            attached_additional_attributes: vec![],
        })
    }

    fn write_to_stream(&self, stream_writer: &mut impl Write) -> Result<()> {
        stream_writer.write_u32::<LittleEndian>(self.attached_digests.iter().map(|digest| digest.calculate_byte_size()).sum())?;
        for single_digest in &self.attached_digests { single_digest.write_to_stream(stream_writer)?; }

        stream_writer.write_u32::<LittleEndian>(self.attached_certificates.iter().map(|certificate| certificate.len() as u32 + 4).sum())?;
        for single_certificate in &self.attached_certificates {
            stream_writer.write_u32::<LittleEndian>(single_certificate.len() as u32)?;
            stream_writer.write_all(single_certificate)?;
        }

        stream_writer.write_u32::<LittleEndian>(self.attached_additional_attributes.iter().map(|(_, attribute_value)| attribute_value.len() as u32 + 8).sum())?;
        for (attribute_identifier, attribute_value) in &self.attached_additional_attributes {
            stream_writer.write_u32::<LittleEndian>(attribute_value.len() as u32 + 4)?;
            stream_writer.write_u32::<LittleEndian>(*attribute_identifier)?;
            stream_writer.write_all(attribute_value)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
struct ApkSignatureBlockV2 {
    pub embedded_signers: Vec<ApkSignerData>,
}

#[derive(Debug)]
struct ApkSignerData {
    pub raw_signed_data: Vec<u8>,
    pub attached_signatures: Vec<ApkSignatureData>,
    pub raw_public_key: Vec<u8>,
}

#[derive(Debug)]
struct ApkSignatureData {
    pub signature_algorithm: u32,
    pub signature_bytes: Vec<u8>,
}

impl ApkSignatureBlockV2 {
    fn new(computed_hash: [u8; 32], active_signer: &Signer) -> Result<Self> {
        let mut generated_signed_data = vec![];
        SignedDataBlock::new(computed_hash, active_signer)?.write_to_stream(&mut generated_signed_data)?;
        let computed_signature = active_signer.execute_signature(&generated_signed_data)?;

        Ok(Self {
            embedded_signers: vec![ApkSignerData {
                raw_signed_data: generated_signed_data,
                attached_signatures: vec![ApkSignatureData {
                    signature_algorithm: RSA_PKCS1V15_SHA2_256,
                    signature_bytes: computed_signature,
                }],
                raw_public_key: active_signer.pubkey().to_public_key_der()?.as_ref().to_vec(),
            }],
        })
    }

    fn write_to_stream(&self, stream_writer: &mut impl Write) -> Result<()> {
        let mut outer_buffer = vec![];
        for single_signer in &self.embedded_signers {
            let mut signer_internal_buffer = vec![];
            signer_internal_buffer.write_u32::<LittleEndian>(single_signer.raw_signed_data.len() as u32)?;
            signer_internal_buffer.write_all(&single_signer.raw_signed_data)?;

            let mut signature_collection_buffer = vec![];
            for single_signature in &single_signer.attached_signatures {
                signature_collection_buffer.write_u32::<LittleEndian>(single_signature.signature_bytes.len() as u32 + 8)?;
                signature_collection_buffer.write_u32::<LittleEndian>(single_signature.signature_algorithm)?;
                signature_collection_buffer.write_u32::<LittleEndian>(single_signature.signature_bytes.len() as u32)?;
                signature_collection_buffer.write_all(&single_signature.signature_bytes)?;
            }
            signer_internal_buffer.write_u32::<LittleEndian>(signature_collection_buffer.len() as u32)?;
            signer_internal_buffer.write_all(&signature_collection_buffer)?;

            signer_internal_buffer.write_u32::<LittleEndian>(single_signer.raw_public_key.len() as u32)?;
            signer_internal_buffer.write_all(&single_signer.raw_public_key)?;

            outer_buffer.write_u32::<LittleEndian>(signer_internal_buffer.len() as u32)?;
            outer_buffer.write_all(&signer_internal_buffer)?;
        }
        stream_writer.write_u32::<LittleEndian>(outer_buffer.len() as u32)?;
        stream_writer.write_all(&outer_buffer)?;
        Ok(())
    }
}

#[derive(Debug, Default)]
struct ApkSignatureBlockInformation {
    pub signing_block_start: u64,
    pub central_directory_start: u64,
    pub end_of_central_directory_start: u64,
}

fn write_apk_signing_block<WriterType: Write + Seek>(
    stream_writer: &mut WriterType,
    computed_hash: [u8; 32],
    active_signer: &Signer,
) -> Result<()> {
    let mut signature_block_buffer = vec![];
    ApkSignatureBlockV2::new(computed_hash, active_signer)?.write_to_stream(&mut signature_block_buffer)?;

    let total_block_size = signature_block_buffer.len() as u64 + 36;
    stream_writer.write_u64::<LittleEndian>(total_block_size)?;
    stream_writer.write_u64::<LittleEndian>(signature_block_buffer.len() as u64 + 4)?;
    stream_writer.write_u32::<LittleEndian>(APK_SIGNING_BLOCK_V2_ID)?;
    stream_writer.write_all(&signature_block_buffer)?;
    stream_writer.write_u64::<LittleEndian>(total_block_size)?;
    stream_writer.write_all(APK_SIGNING_BLOCK_MAGIC)?;

    Ok(())
}

fn parse_apk_signing_block<ReaderType: Read + Seek>(stream_reader: &mut ReaderType) -> Result<ApkSignatureBlockInformation> {
    let extracted_zip_info = ZipInfo::new(stream_reader)?;
    let mut block_information = ApkSignatureBlockInformation {
        end_of_central_directory_start: extracted_zip_info.end_of_central_directory_start,
        central_directory_start: extracted_zip_info.central_directory_start,
        ..Default::default()
    };

    stream_reader.seek(SeekFrom::Start(block_information.central_directory_start - 16 - 8))?;
    let extracted_remaining_size = stream_reader.read_u64::<LittleEndian>()?;
    let mut magic_verification_buffer = [0; 16];
    stream_reader.read_exact(&mut magic_verification_buffer)?;

    if magic_verification_buffer != APK_SIGNING_BLOCK_MAGIC {
        block_information.signing_block_start = block_information.central_directory_start;
        return Ok(block_information);
    }

    let calculated_current_position = stream_reader.seek(SeekFrom::Current(-(extracted_remaining_size as i64)))?;
    block_information.signing_block_start = calculated_current_position - 8;

    Ok(block_information)
}