"""Tanda tangan update Ovner (Ed25519).

Pesan yang ditandatangani: "ovner-update\n{versi}\n{build}\n{sha256}".
Kunci privat (seed 32 byte, base64) dari variabel lingkungan OVNER_SIGN_KEY (secret GitHub).
Mencetak tanda tangan base64; kosong bila kunci tidak ada.
"""
import base64
import os
import sys

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey


def main() -> None:
    version, build, sha256 = sys.argv[1], sys.argv[2], sys.argv[3].lower()
    seed = os.environ.get("OVNER_SIGN_KEY", "").strip()
    if not seed:
        print("")
        return
    key = Ed25519PrivateKey.from_private_bytes(base64.b64decode(seed))
    msg = f"ovner-update\n{version}\n{build}\n{sha256}".encode()
    print(base64.b64encode(key.sign(msg)).decode())


if __name__ == "__main__":
    main()
