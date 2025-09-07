import os
from pathlib import Path
print(os.getcwd())
import subprocess
import shutil

# Build PFP_Writer
subprocess.run(
    ["cargo", "build", "--release"],
    cwd=r"C:\Users\danie\Documents\GitHub\PFP 3N DCS Bios [important]"
)

# Move .exe to rust_gui folder
shutil.copy2(
    r"C:\Users\danie\Documents\GitHub\PFP 3N DCS Bios [important]\target\release\pfp_writer.exe",
    r"C:\Users\danie\Documents\GitHub\UWACS\children"
)

# Move output4 to rust_gui folder
shutil.copy2(
    r"C:\Users\danie\Documents\GitHub\PFP 3N DCS Bios [important]\target\release\output4.txt",
    r"C:\Users\danie\Documents\GitHub\UWACS\children"
)


# Build UFC_Writer
subprocess.run(
    ["cargo", "build", "--release"],
    cwd=r"C:\Users\danie\Documents\GitHub\ufc_writer [important]\rust"
)

# Move .exe to rust_gui folder
shutil.copy2(
    r"C:\Users\danie\Documents\GitHub\ufc_writer [important]\rust\target\release\ufc_writer.exe",
    r"C:\Users\danie\Documents\GitHub\UWACS\children"
)

# Build UWACS
subprocess.run(
    "npm run tauri build",
    cwd=r"C:\Users\danie\Documents\GitHub\UWACS",
    shell=True
)

# Move UWACS Exe to folder
shutil.copy2(
    r"C:\Users\danie\Documents\GitHub\UWACS\src-tauri\target\release\uwacs.exe",
    r"C:\Users\danie\Documents\GitHub\UWACS\custom_out\Main"
)

# Move Deps folder
shutil.copytree(
    r"C:\Users\danie\Documents\GitHub\UWACS\src-tauri\target\release\deps",
    r"C:\Users\danie\Documents\GitHub\UWACS\custom_out\Main\deps",
    dirs_exist_ok=True  # available in Python 3.8+
)

# Move Resources folder
shutil.copytree(
    r"C:\Users\danie\Documents\GitHub\UWACS\src-tauri\target\release\resources",
    r"C:\Users\danie\Documents\GitHub\UWACS\custom_out\Main\resources",
    dirs_exist_ok=True  # available in Python 3.8+
)

# Move output4
shutil.copy2(
    r"C:\Users\danie\Documents\GitHub\PFP 3N DCS Bios [important]\target\release\output4.txt",
    r"C:\Users\danie\Documents\GitHub\UWACS\custom_out\Main"
)

# Move PFP exe
shutil.copy2(
    r"C:\Users\danie\Documents\GitHub\PFP 3N DCS Bios [important]\target\release\pfp_writer.exe",
    r"C:\Users\danie\Documents\GitHub\UWACS\custom_out\children"
)

# Move UFC exe
shutil.copy2(
    r"C:\Users\danie\Documents\GitHub\ufc_writer [important]\rust\target\release\ufc_writer.exe",
    r"C:\Users\danie\Documents\GitHub\UWACS\custom_out\children"
)


# Move FluidPort exe
shutil.copy2(
    r"C:\Users\danie\Documents\GitHub\UWACS\children\FluidPort.exe",
    r"C:\Users\danie\Documents\GitHub\UWACS\custom_out\children"
)

# Move Image
shutil.copy2(
    r"C:\Users\danie\Documents\GitHub\UWACS\children\AH-64D_MFCD_leaderLine.png",
    r"C:\Users\danie\Documents\GitHub\UWACS\custom_out\Main"
)

folder_to_zip = r"C:\Users\danie\Documents\GitHub\UWACS\custom_out"
output_zip = r"C:\Users\danie\Documents\GitHub\UWACS\main_archive"
shutil.make_archive(output_zip, 'zip', folder_to_zip)