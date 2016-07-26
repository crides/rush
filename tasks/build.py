"""
Build tasks.
"""
from __future__ import print_function

from itertools import chain, imap
import logging
from pathlib import Path
try:
    from shlex import quote
except ImportError:
    from pipes import quote
import shutil
import sys

from invoke import task
import semver

from tasks import BIN, LIB
from tasks.util import cargo
from tasks.util.docs import \
    read_mkdocs_config, describe_rust_api, insert_api_docs


MIN_RUSTC_VERSION = '1.10.0'

HELP = {
    'release': "Whether to build artifacts in release mode",
    'verbose': "Whether to show verbose logging output of the build",
}


@task(help=HELP, default=True)
def all(ctx, release=False, verbose=False):
    """Build the project."""
    # calling lib() is unnecessary because the binary crate
    # depeends on the library, so it will be rebuilt as well
    bin(ctx, release=release, verbose=verbose)
    docs(ctx, release=release, verbose=verbose)
    print("\nBuild finished.", file=sys.stderr)


@task(help=HELP)
def bin(ctx, release=False, verbose=False):
    """Build the binary crate."""
    ensure_rustc_version(ctx)
    cargo(ctx, 'build', *get_rustc_flags(release, verbose),
          crate=BIN, pty=True)


@task(help=HELP)
def lib(ctx, release=False, verbose=False):
    """Build the library crate."""
    ensure_rustc_version(ctx)
    cargo(ctx, 'build', *get_rustc_flags(release, verbose),
          crate=LIB, pty=True)


@task(help=HELP)
def docs(ctx, release=False, verbose=False, dump_api=False):
    """Build the project documentation.

    This includes analyzing the Rust modules that implement expression API
    in order to extract their in-code documentation before putting it in
    the dedicated documentation page as Markdown.

    It also removes some of the superfluous files from the docs output
    directory in release mode.
    """
    # describe the API modules and functions contained therein,
    # rendering the documentation as Markdown into the designated doc page
    is_root_mod_rs = lambda p: p.stem == 'mod' and p.parent.stem == 'api'
    module_paths = [
        mod for mod in Path('./crates', LIB, 'src/eval/api').rglob('**/*.rs')
        if not is_root_mod_rs(mod)]
    modules = describe_rust_api(*module_paths)
    insert_api_docs(modules, into='./docs/api.md')

    # build the docs in output format
    args = ['--strict']
    if release:
        args.append('--clean')
    if verbose:
        args.append('--verbose')
    build_run = ctx.run('mkdocs build ' + ' '.join(map(quote, args)), pty=True)
    if not build_run.ok:
        logging.fatal("mkdocs build failed, aborting.")
        sys.exit(1)

    # for release doc builds, clean some of the output files that get
    # copied verbatim since mkdocs doesn't support ignoring them
    if release:
        mkdocs_config = read_mkdocs_config()

        # read the list of ignored path patterns from a file
        ignored = []
        ignore_file = (
            Path.cwd() / mkdocs_config.get('docs_dir', 'docs') / '.docsignore'
        )
        if ignore_file.exists():
            if verbose:
                logging.info(
                    "%s file found, applying ignore patterns...", ignore_file)
            with ignore_file.open(encoding='utf-8') as f:
                ignored = [
                    line.rstrip() for line in f.readlines()
                    if line.strip() and not line.lstrip().startswith('#')]
        else:
            if verbose:
                logging.info("%s not found, not removing any ignored files.",
                             ignore_file)

        # resolve the patterns to see what files in the output dir
        # they correspond to, if any
        if ignored:
            output_dir = Path.cwd() / mkdocs_config.get('site_dir', 'site')
            ignored = chain.from_iterable(imap(output_dir.glob, ignored))

        # "ignore" them, i.e. delete from output directory
        for path in ignored:
            if verbose:
                logging.info("Removing ignored file/directory '%s'", path)
            if path.is_dir():
                shutil.rmtree(str(path))
            else:
                path.unlink()


# Utility functions

def ensure_rustc_version(ctx):
    """Terminates the build unless the Rust compiler is recent enough."""
    rustc_v = ctx.run('rustc --version', hide=True)
    if not rustc_v.ok:
        logging.critical("Rust compiler not found, aborting build.")
        sys.exit(127)

    _, version, _ = rustc_v.stdout.split(None, 2)
    if not semver.match(version, '>=' + MIN_RUSTC_VERSION):
        logging.error("Build requires at least Rust %s, found %s",
                      MIN_RUSTC_VERSION, version)
        sys.exit(1)

    return True


def get_rustc_flags(release, verbose):
    """Return a list of Rust compiler flags corresponding to given params."""
    flags = []
    if release:
        flags.append('--release')
    if verbose:
        flags.append('--verbose')
    return flags
