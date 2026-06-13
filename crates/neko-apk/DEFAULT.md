# Default Workspace Guide
Here is how to utilize the default workspace to create patched APKs:

* `mod/patch/`: Every file you'd like to overwrite within the game's pack files goes here
* `mod/loose/`: Every file within the games `assets/` folder that is not within a pack that you would like to overwrite goes here
* `mod/icons/`: Custom `icon.png`, `icon_foreground.png`, and `push_icon.png` app assets go here
* `mod/code/`: Modified `lib{}.so` code goes here, make sure to set an architecture in the config or else it will skip these files

APKs  will be Created in the `apk` directory, with the original APK staying in-tact

If you dislike the workflow, binary behavior, or are looking for additional options, you may change the config or use flags within the command-line