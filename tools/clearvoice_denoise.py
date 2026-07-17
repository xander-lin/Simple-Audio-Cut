import argparse

from clearvoice import ClearVoice


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", required=True)
    parser.add_argument("--input", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()

    engine = ClearVoice(task="speech_enhancement", model_names=[args.model])
    enhanced = engine(input_path=args.input, online_write=False)
    engine.write(enhanced, output_path=args.output)


if __name__ == "__main__":
    main()
