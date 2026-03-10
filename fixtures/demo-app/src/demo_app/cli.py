import sys


def main() -> int:
    print("demo-args:" + ",".join(sys.argv[1:]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
