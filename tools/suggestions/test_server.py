import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


class WorkerPrivacyTests(unittest.TestCase):
    def test_runtime_diagnostics_are_not_written_to_logs(self):
        with tempfile.TemporaryDirectory() as directory:
            script = '''
import server, sys
def noisy_start(self, deadline):
    print("private-tensor-sentinel", flush=True)
    print("private-tensor-sentinel", file=sys.stderr, flush=True)
    raise ValueError("private-tensor-sentinel")
server.Inference.start = noisy_start
sys.argv = ["server.py", "--socket", sys.argv[1]]
server.main()
'''
            result = subprocess.run(
                [sys.executable, '-c', script, str(Path(directory) / 'worker.sock')],
                cwd=Path(__file__).parent, capture_output=True, timeout=10, check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, b'')
            self.assertEqual(result.stderr, b'')


if __name__ == '__main__':
    unittest.main()
