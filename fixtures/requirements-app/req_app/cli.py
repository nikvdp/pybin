import click


@click.command()
@click.argument("items", nargs=-1)
def main(items: tuple[str, ...]) -> None:
    rendered = ",".join(items)
    click.echo(f"requirements-app:{rendered}")


if __name__ == "__main__":
    main()
