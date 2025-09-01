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
