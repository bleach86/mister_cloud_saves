#!/usr/bin/env python

# Copyright (c) 2025-2026 bleach86

# This program is free software: you can redistribute it and/or modify
# it under the terms of the GNU General Public License as published by
# the Free Software Foundation, either version 3 of the License, or
# (at your option) any later version.

# This program is distributed in the hope that it will be useful,
# but WITHOUT ANY WARRANTY; without even the implied warranty of
# MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
# GNU General Public License for more details.

# You should have received a copy of the GNU General Public License
# along with this program.  If not, see <http://www.gnu.org/licenses/>.

# You can download the latest version of this tool from:
# https://github.com/bleach86/mister_cloud_saves

import os
import subprocess
import sys
import shutil


MISTER_PATH = "/media/fat"
CLIENT_DIR = os.path.join(MISTER_PATH, "cloud_saves")
UPDATES_DIR = os.path.join(CLIENT_DIR, "updates")


def check_updates():
    """
    Checks for updates to the Mister Cloud Saves Client.
    """
    update_file = os.path.join(UPDATES_DIR, "client.tar.xz")

    if os.path.isfile(update_file):
        extract_client(update_file)


def extract_client(update_file_path):
    """
    Extracts the client archive to the client directory.
    """
    if os.path.isfile(update_file_path):
        if not os.path.isdir(CLIENT_DIR):
            os.makedirs(CLIENT_DIR)

        shutil.unpack_archive(update_file_path, CLIENT_DIR, format="xztar")
        os.remove(update_file_path)
    else:
        print("Client archive not found")
        sys.exit(1)


def run_client():
    """
    Runs the Mister Cloud Saves Client.
    """
    client_executable = os.path.join(CLIENT_DIR, "mister_save_client")

    if not os.path.isfile(client_executable):
        print("Mister Cloud Saves Client executable not found.")
        sys.exit(1)

    subprocess.Popen(
        [client_executable],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        stdin=subprocess.DEVNULL,
        start_new_session=True,
        close_fds=True,
    )


def main():
    """
    Main function to check for updates and run the Mister Cloud Saves Client.
    """

    check_updates()
    run_client()


if __name__ == "__main__":
    main()
