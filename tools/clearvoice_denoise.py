import argparse
from contextlib import redirect_stdout
import importlib
import json
import os
import sys

DEVICE_ENV = "SIMPLE_AUDIO_CUT_CLEARVOICE_DEVICE"
PROTOCOL_PREFIX = "SIMPLE_AUDIO_CUT_WORKER="


def normalized_device(value):
    device = (value or "auto").strip().lower()
    if not device or device == "auto":
        return "auto"
    if device == "gpu":
        return "cuda"
    if device in {"cpu", "cuda"}:
        return device
    if device.startswith("cuda:") and device.split(":", 1)[1].isdigit():
        return device
    raise ValueError("ClearVoice device must be auto, cpu, cuda, cuda:N, or gpu.")


def configure_device_environment(device):
    device = normalized_device(device)
    if device == "auto":
        device = "cuda" if cuda_works() else "cpu"
    if device == "cpu":
        os.environ["CUDA_VISIBLE_DEVICES"] = ""
    elif device.startswith("cuda:"):
        os.environ["CUDA_VISIBLE_DEVICES"] = device.split(":", 1)[1]
    return device


def cuda_works():
    try:
        validate_device("cuda")
        return True
    except Exception:
        return False


def validate_device(device):
    if device == "auto":
        validate_device(configure_device_environment(device))
        return
    if device == "cpu":
        return
    torch = importlib.import_module("torch")

    if not torch.cuda.is_available():
        raise RuntimeError("CUDA was requested for ClearVoice, but PyTorch cannot access a CUDA device.")
    torch.cuda.set_device(0)
    probe = torch.ones((1,), device="cuda") + 1
    if probe.cpu().item() != 2:
        raise RuntimeError("CUDA probe returned an unexpected result.")


def emit_protocol(stream, payload):
    stream.write(PROTOCOL_PREFIX + json.dumps(payload, ensure_ascii=True) + "\n")
    stream.flush()


def serve(device):
    protocol_stream = sys.stdout
    with redirect_stdout(sys.stderr):
        device = configure_device_environment(device)
        validate_device(device)
        ClearVoice = importlib.import_module("clearvoice").ClearVoice

    engines = {}
    emit_protocol(protocol_stream, {"status": "ready"})
    for line in sys.stdin:
        request_id = None
        try:
            request = json.loads(line)
            request_id = request.get("id")
            if request.get("operation") == "shutdown":
                emit_protocol(protocol_stream, {"id": request_id, "status": "stopped"})
                return
            if request.get("operation") != "enhance":
                raise ValueError("Unsupported ClearVoice worker operation.")

            model = request["model"]
            loaded_model = model not in engines
            with redirect_stdout(sys.stderr):
                if loaded_model:
                    engines[model] = ClearVoice(task="speech_enhancement", model_names=[model])
                engine = engines[model]
                enhanced = engine(input_path=request["input"], online_write=False)
                engine.write(enhanced, output_path=request["output"])
            emit_protocol(protocol_stream, {
                "id": request_id,
                "status": "complete",
                "loadedModel": loaded_model,
            })
        except Exception as error:
            emit_protocol(protocol_stream, {
                "id": request_id,
                "status": "error",
                "error": f"{type(error).__name__}: {error}",
            })


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--serve", action="store_true")
    parser.add_argument("--model")
    parser.add_argument("--input")
    parser.add_argument("--output")
    parser.add_argument("--device", default=os.environ.get(DEVICE_ENV, "auto"))
    args = parser.parse_args()

    try:
        if args.serve:
            serve(args.device)
            return
        if not args.model or not args.input or not args.output:
            parser.error("--model, --input, and --output are required unless --serve is used")
        device = configure_device_environment(args.device)
        validate_device(device)

        ClearVoice = importlib.import_module("clearvoice").ClearVoice

        engine = ClearVoice(task="speech_enhancement", model_names=[args.model])
        enhanced = engine(input_path=args.input, online_write=False)
        engine.write(enhanced, output_path=args.output)
    except Exception as error:
        if args.serve:
            emit_protocol(sys.stdout, {
                "status": "error",
                "error": f"{type(error).__name__}: {error}",
            })
        print(f"{type(error).__name__}: {error}", file=sys.stderr)
        raise SystemExit(1) from error


if __name__ == "__main__":
    main()
