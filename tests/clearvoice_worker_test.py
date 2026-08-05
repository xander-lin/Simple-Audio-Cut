import contextlib
import io
import json
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import patch

from tools import clearvoice_denoise


class FakeClearVoice:
    constructions = []
    calls = []

    def __init__(self, task, model_names):
        self.model = model_names[0]
        self.constructions.append((task, self.model))

    def __call__(self, input_path, online_write):
        self.calls.append((self.model, input_path, online_write))
        return input_path

    def write(self, enhanced, output_path):
        Path(output_path).write_text(f"{self.model}:{enhanced}", encoding="utf-8")


class ClearVoiceWorkerTest(unittest.TestCase):
    def setUp(self):
        FakeClearVoice.constructions.clear()
        FakeClearVoice.calls.clear()

    def test_server_reuses_each_model_engine(self):
        requests = []
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for index, model in enumerate(("48k", "48k", "16k", "16k"), start=1):
                source = root / f"input-{index}.wav"
                source.write_text("audio", encoding="utf-8")
                requests.append(json.dumps({
                    "id": index,
                    "operation": "enhance",
                    "model": model,
                    "input": str(source),
                    "output": str(root / f"output-{index}.wav"),
                }))
            requests.append(json.dumps({"id": 5, "operation": "shutdown"}))
            protocol = io.StringIO()
            fake_module = types.SimpleNamespace(ClearVoice=FakeClearVoice)
            with patch.object(clearvoice_denoise, "configure_device_environment", return_value="cpu"), \
                    patch.object(clearvoice_denoise, "validate_device"), \
                    patch.dict(sys.modules, {"clearvoice": fake_module}), \
                    patch.object(sys, "stdin", io.StringIO("\n".join(requests) + "\n")), \
                    patch.object(sys, "stdout", protocol), \
                    contextlib.redirect_stderr(io.StringIO()):
                clearvoice_denoise.serve("cpu")

            responses = [
                json.loads(line.split("=", 1)[1])
                for line in protocol.getvalue().splitlines()
            ]
            self.assertEqual([response["status"] for response in responses], [
                "ready", "complete", "complete", "complete", "complete", "stopped",
            ])
            self.assertEqual([response.get("loadedModel") for response in responses[1:5]], [
                True, False, True, False,
            ])
            self.assertEqual(FakeClearVoice.constructions, [
                ("speech_enhancement", "48k"),
                ("speech_enhancement", "16k"),
            ])
            self.assertEqual(len(FakeClearVoice.calls), 4)


if __name__ == "__main__":
    unittest.main()
