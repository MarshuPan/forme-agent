"""Exercise the real Git checkout boundary for content-addressed Web resources."""
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
ASSET_ROOT = "crates/gateway/assets/product"
FILES = {
    "frontend/index.html": b"<!doctype html>\n<html></html>\n",
    "frontend/public/favicon.svg": b"<svg>\n</svg>\n",
    "frontend/src/main.tsx": b"const fixture = 'checkout';\n",
    f"{ASSET_ROOT}/embedded.rs": b"// generated route table\nfn asset() {}\n",
    f"{ASSET_ROOT}/dist/index.html": b"<!doctype html>\n<html></html>\n",
    f"{ASSET_ROOT}/dist/assets/index-fixture.css": b"body { color: black; }\n",
    f"{ASSET_ROOT}/dist/assets/index-fixture.js": b"console.log('fixture');\n",
    f"{ASSET_ROOT}/dist/LICENSES.txt": b"fixture license\nsecond line\n",
    f"{ASSET_ROOT}/dist/manifest.json": b'{\n  "schema_version": 1\n}\n',
    f"{ASSET_ROOT}/dist/dependencies.json": b'{\n  "dependencies": []\n}\n',
    f"{ASSET_ROOT}/dist/favicon.svg": b"<svg>\n</svg>\n",
}


class WebAssetCheckoutTests(unittest.TestCase):
    def checkout(self, attributes: bytes) -> dict[str, bytes]:
        with tempfile.TemporaryDirectory(prefix="forme-web-checkout-") as directory:
            root = Path(directory)

            def git(*arguments: str) -> bytes:
                result = subprocess.run(
                    ["git", "-C", str(root), *arguments],
                    capture_output=True,
                    timeout=30,
                    check=False,
                )
                self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))
                return result.stdout

            git("init", "--quiet")
            git("config", "core.autocrlf", "true")
            git("config", "core.safecrlf", "false")
            (root / ".gitattributes").write_bytes(attributes)
            for name, data in FILES.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
            git("add", "--", ".gitattributes", "frontend", ASSET_ROOT)
            git("-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid",
                "-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "fixture")
            for name in FILES:
                (root / name).unlink()
            git("restore", "--source=HEAD", "--worktree", "--", "frontend", ASSET_ROOT)
            return {name: (root / name).read_bytes() for name in FILES}

    def test_content_addressed_resources_survive_windows_style_git_checkout(self):
        self.assertEqual(self.checkout((ROOT / ".gitattributes").read_bytes()), FILES)

    def test_unprotected_checkout_reproduces_the_byte_drift(self):
        checkout = self.checkout(b"# no content-addressed resource protection\n")
        self.assertNotEqual(checkout, FILES)
        self.assertIn(b"\r\n", checkout[f"{ASSET_ROOT}/dist/assets/index-fixture.css"])


if __name__ == "__main__":
    unittest.main()
