use crate::io::get_local_dir;
use std::fs;

const README_CONTENT: &str = "# Default Workspace Guide
Here is how to utilize the default workspace to create patched APKs:

* `mod/patch/`: Every file you'd like to overwrite within the game's pack files goes here
* `mod/loose/`: Every file within the games `assets/` folder that is not within a pack that you would like to overwrite goes here
* `mod/icons/`: Custom `icon.png`, `icon_foreground.png`, and `push_icon.png` app assets go here

APKs with differing App or Package names will be Created in the `apk` directory, with the original APK staying in-tact

APKs that have the same App and Package name as their input will overwrite the original input APK upon creation

If you dislike the workflow, binary behavior, or are looking for additional options, you may change the config or use flags within the command-line
";

pub fn generate() -> std::io::Result<()> {
    let mut path = get_local_dir();
    path.push("README.md");
    fs::write(path, README_CONTENT)
}