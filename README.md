# Mister Cloud Saves

A utility to sync MiSTer FPGA save files with a cloud server.

## Features

- Sync save files to/from a cloud server
- Support for multiple MiSTer devices syncing to the same server
- Syncs saves and save states
- Core agnostic - works with any MiSTer core that uses save files
- Conflict resolution for multiple devices

## Installation

Grab the [`cloud_saves.sh`](https://github.com/bleach86/mister_cloud_saves/blob/main/scripts/cloud_saves.sh) file from the `scripts` directory in this repository and place it in the `Scripts` folder of your MiSTer FPGA's SD card.

Boot up your MiSTer. From the MiSTer menu:

1. Press the **back** action (Esc key on a keyboard).
2. Select the **Scripts** option.
3. Choose **Yes** to allow running scripts.
4. Select the `cloud_saves` script to run it.

This will automatically begin the installation and initial sync process.

- **Using the provided cloud server:** Simply press **Enter** when prompted for the server URL.
- **Hosting your own server:** Enter the full URL to your server when prompted.

Examples:
`https://example.com`
`http://192.168.1.45:8000`

> Your server must be running and reachable from the MiSTer for the sync to work.

The script will now begin syncing with the server. Once complete, the MiSTer will reboot automatically.

- **Single MiSTer device:** You’re done! The `mister_save_client` will automatically run in the background on boot and sync your saves.
- **Multiple MiSTer devices:** There are a couple of additional steps to complete (see below).

### Multiple MiSTer Devices

During the initial setup on the first MiSTer, an `.ini` file named `cloud_saves.ini` is created in the root of the SD card.  
Copy this file to the root of the SD card on each additional MiSTer device you want to sync with the same cloud server.

Each device will also need the  
[`cloud_saves.sh`](https://github.com/bleach86/mister_cloud_saves/blob/main/scripts/cloud_saves.sh) script placed in the `Scripts` folder.

Once copied, run the **cloud_saves** script from the MiSTer menu on each additional device. The script will detect the existing `cloud_saves.ini` file and automatically use the same server URL and user ID as the first device.

During the initial sync from the second device onward, if a save file already exists on the server but has a different hash than the local file, a conflict will be detected. You will be prompted to choose which version of the file to keep.

You can choose to:

- Keep the **local** version
- Keep the **server** version

If you choose to keep the local version, it will be uploaded to the server and overwrite the existing server file. This updated version will then be synced to all other devices during their next sync.

You will also have the option to apply the same choice to all remaining conflicts.

> ⚠️ **Important:** If you play the same game on multiple MiSTer devices at the same time, save data could be **overwritten**. Always make sure only one device is actively playing or saving a game at a time to avoid conflicts.

## Usage

After the initial setup, the `mister_save_client` will automatically run in the background on MiSTer at boot and sync your save files with the cloud server.

## Updating

To update to the latest version, simply run the `cloud_saves` script again from the MiSTer menu. When prompted, choose the update option. The script will download and install the latest version of the client and perform a sync.

## Uninstallation

To uninstall the `mister_save_client`, run the `cloud_saves` script from the MiSTer menu and choose the uninstall option. This will remove the client and all associated files from your MiSTer SD card.

## License

This project is licensed under the GPLv3 License. See the [LICENSE](LICENSE) file for details.
