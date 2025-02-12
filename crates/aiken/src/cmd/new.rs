use aiken_project::{
    config::Config,
    package_name::{self, PackageName},
};
use indoc::{formatdoc, indoc};
use miette::IntoDiagnostic;
use owo_colors::{OwoColorize, Stream::Stderr};
use std::{
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

#[derive(clap::Args)]
/// Create a new Aiken project
pub struct Args {
    /// Project name
    name: String,
    /// Library only
    #[clap(long)]
    lib: bool,
}

pub fn exec(args: Args) -> miette::Result<()> {
    let package_name = PackageName::from_str(&args.name).into_diagnostic()?;
    create_project(args, &package_name)?;
    print_success_message(&package_name);
    Ok(())
}

fn create_project(args: Args, package_name: &PackageName) -> miette::Result<()> {
    let root = PathBuf::from(&package_name.repo);

    if root.exists() {
        Err(package_name::Error::ProjectExists {
            name: package_name.repo.clone(),
        })?;
    }

    create_lib_folder(&root, package_name)?;

    if !args.lib {
        create_validators_folder(&root)?;
    }

    readme(&root, &package_name.repo)?;

    Config::default(package_name)
        .save(&root)
        .into_diagnostic()?;

    gitignore(&root)?;

    Ok(())
}

fn print_success_message(package_name: &PackageName) {
    eprintln!(
        "\n{}",
        formatdoc! {
            r#"Your Aiken project {name} has been {s} created.
               The project can be compiled and tested by running these commands:

                   {cd} {name}
                   {aiken} check
            "#,
            s = "successfully"
                .if_supports_color(Stderr, |s| s.bright_green())
                .if_supports_color(Stderr, |s| s.bold()),
            cd = "cd"
                .if_supports_color(Stderr, |s| s.purple())
                .if_supports_color(Stderr, |s| s.bold()),
            name = package_name
                .if_supports_color(Stderr, |s| s.repo.bright_blue()),
            aiken = "aiken"
                .if_supports_color(Stderr, |s| s.purple())
                .if_supports_color(Stderr, |s| s.bold())
        }
    )
}

fn create_lib_folder(root: &Path, package_name: &PackageName) -> miette::Result<()> {
    let lib = root.join("lib");
    fs::create_dir_all(&lib).into_diagnostic()?;
    let nested_path = lib.join(&package_name.repo);
    fs::create_dir_all(nested_path).into_diagnostic()?;
    Ok(())
}

fn create_validators_folder(root: &Path) -> miette::Result<()> {
    let validators = root.join("validators");
    fs::create_dir_all(validators).into_diagnostic()?;
    Ok(())
}

fn readme(root: &Path, project_name: &str) -> miette::Result<()> {
    fs::write(
        root.join("README.md"),
        formatdoc! {
            r#"
                # {name}

                Write validators in the `validators` folder, and supporting functions in the `lib` folder using `.ak` as a file extension.

                For example, as `validators/always_true.ak`

                ```gleam
                validator {{
                  fn spend(_datum: Data, _redeemer: Data, _context: Data) -> Bool {{
                    True
                  }}
                }}
                ```

                Validators are named after their purpose, so one of:

                - `spent`
                - `mint`
                - `withdraw`
                - `publish`

                ## Building

                ```sh
                aiken build
                ```

                ## Testing

                You can write tests in any module using the `test` keyword. For example:

                ```gleam
                test foo() {{
                  1 + 1 == 2
                }}
                ```

                To run all tests, simply do:

                ```sh
                aiken check
                ```

                To run only tests matching the string `foo`, do:

                ```sh
                aiken check -m foo
                ```

                ## Documentation

                If you're writing a library, you might want to generate an HTML documentation for it.

                Use:

                ```sh
                aiken docs
                ```

                ## Resources

                Find more on the [Aiken's user manual](https://aiken-lang.org).
            "#,
            name = project_name
        },
    ).into_diagnostic()
}

fn gitignore(root: &Path) -> miette::Result<()> {
    let gitignore_path = root.join(".gitignore");

    fs::write(
        gitignore_path,
        indoc! {
            r#"
                # Aiken compilation artifacts
                artifacts/
                # Aiken's project working directory
                build/
                # Aiken's default documentation export
                docs/
            "#
        },
    )
    .into_diagnostic()?;

    Ok(())
}
