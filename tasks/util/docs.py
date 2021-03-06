"""
Helper code related to generating documentation for the project.
"""
from collections import namedtuple, OrderedDict
from itertools import repeat, takewhile
import logging
import os
from pathlib import Path
import re
import sys

import jinja2
import yaml
from glob2 import glob


__all__ = [
    'read_mkdocs_config', 'get_docs_output_dir', 'scrub_html_comment_markers',
    'describe_rust_api', 'insert_api_docs',
]


def read_mkdocs_config():
    """Read the Mkdocs configuration file.
    :return: Dictionary of Mkdocs config options
    """
    base_dir = Path.cwd()

    for filename in ('mkdocs.yml', 'mkdocs.yaml'):
        config_file = base_dir / filename
        if not config_file.exists():
            continue
        with config_file.open(encoding='utf-8') as f:
            return yaml.load(f)

    logging.error("Mkdocs config file cannot be found; "
                  "is it the project's root directory?")
    sys.exit(1)


def get_docs_output_dir():
    """Retrieve the full path to the documentation's output directory.
    :return: Docs output directory as Path object
    """
    config = read_mkdocs_config()
    return Path.cwd() / config.get('site_dir', 'site')


def scrub_html_comment_markers(filepath):
    """Scrub lines with HTML comments from given file.

    Only lines that contain HTML comments and nothing else are removed.
    """
    path = Path(filepath)

    def is_marker_line(l):
        return l.lstrip().startswith('<!--') and l.rstrip().endswith('-->')

    with path.open('r+t', encoding='utf-8') as f:
        lines = [line for line in f.readlines() if not is_marker_line(line)]

        f.seek(0)
        f.truncate()
        f.writelines(lines)


# Generating API docs

#: Jinja environment for rendering the API modules' Markdown docs.
#: The **output** here is Markdown; it is then further processed by mkdocs.
#:
#: The template loader assumes the code is ran from project's root directory.
jinja_env = jinja2.Environment(
    loader=jinja2.FileSystemLoader(str(Path.cwd() / 'docs' / 'partials')))


#: Module under eval::api that contains API functions.
class Module(namedtuple('Module', [
    'path',  # full path to the module file
    'name',
    'description',  # module-level docstring
    'submodules',  # list of Module objects
    'functions',  # list of Function objects
])):
    def render(self):
        """Render the module as Markdown source."""
        template = jinja_env.get_template('module.md.jinja')
        return template.render({'mod': self})


#: API function that should be described in the end-user documentation.
class Function(namedtuple('Function', [
    'name',
    'description',  # general description of the function
    'arguments',  # OrderedDict of names->descriptions
    'returns',  # description of the return value
])):
    def render(self):
        """Render the function as Markdown source."""
        template = jinja_env.get_template('function.md.jinja')
        return template.render({'func': self})


def insert_api_docs(modules,
                    into, between=('BEGIN AUTOGENERATED API DOCUMENTATION',
                                   'END AUTOGENERATED API DOCUMENTATION')):
    """Render the docs for expression API into given file.

    :param modules: Iterable of Module objects
    :param into: Path to the Markdown file to render the API docs to
    :param between: Tuple of delimiter strings that mark the lines
                    that will be replaced with docs

    :raise ValueError:
        * if the line markers are invalid
        * if the target file doesn't contain the line markers
    """
    if not modules:
        return

    docs = os.linesep.join(module.render() for module in modules)

    target_path = Path(into)
    with target_path.open('r+t', encoding='utf-8') as f:
        target_lines = [line.strip() for line in f.readlines()]

        # determine the line indices of the region to replace
        begin_marker, end_marker = between
        begin_idx, end_idx = None, None
        for idx, line in enumerate(target_lines):
            if begin_marker in line:
                begin_idx = idx
            if end_marker in line:
                end_idx = idx
        if begin_idx is None or end_idx is None:
            raise ValueError(
                "begin or end marker not found in %s (begin:%s, end:%s)" % (
                    target_path, begin_idx, end_idx))
        if begin_idx == end_idx:
            raise ValueError("empty region")

        # format the final content of the file, with docs inserted
        # between markers (the outer os.linesep.join will also add empty line
        # between docs and the rest of the file for better
        # Markdown compatibility)
        target_content = os.linesep.join([
            os.linesep.join(target_lines[:begin_idx + 1]).strip(),
            docs,
            os.linesep.join(target_lines[end_idx:]).strip(),
        ])

        f.seek(0)
        f.truncate()
        f.write(target_content)


def describe_rust_api(*src):
    """Describe the public API implemented in given Rust modules.

    :param src: Rust source file(s) to analyze.
                This can be a list of file paths or a glob string.

    :return: Iterable of Module objects
    """
    # collapse the possible lists of files into a single list
    sources = [[s] if isinstance(s, (str, Path)) else s for s in src]
    sources = sum(sources, [])
    if not sources:
        return

    for source in sources:
        for filename in glob(str(source)):
            module = analyze_rust_module(filename)
            if module:
                yield module


def analyze_rust_module(path):
    """Analyze given Rust module file.

    The analysis looks for public functions defined in the module
    and extracts their names, arguments, and documentation.

    :param path: Path to the module
    :return: Module object or None
    """
    path = Path(path)
    logging.info("Analyzing Rust module %s...", path)

    # extract module name & potentially analyze submodules
    mod_name = path.stem
    submodules = []
    if mod_name == 'mod':  # does this module have submodules?
        if path.parent:
            mod_name = path.parent.stem  # Rust rule: foo.rs == foo/mod.rs
            for submodule_path in path.parent.glob('*.rs'):
                if submodule_path.stem != 'mod':
                    submodules.extend(describe_rust_api(str(submodule_path)))
        else:
            # bare "mod.rs" as module path, rather unlikely occurrence
            mod_name = ''

    with path.open(encoding='utf-8') as f:
        lines = f.readlines()

    # extract module-level docstring if available
    mod_docstring = os.linesep.join(
        line.lstrip('/!').strip()
        for line in takewhile(lambda l: l.startswith('//!'), lines))

    # analyze the function declarations and extract info on them
    functions = []
    pub_fn_line_indices = (i for i, line in enumerate(lines)
                           if line.lstrip().startswith('pub fn'))
    for idx in pub_fn_line_indices:
        def_start_line = lines[idx]
        if re.search(r'\bself\b', def_start_line):
            continue  # omit methods

        # extract function name
        fn_name_match = re.match(r'pub\s+fn\s+(\w+)\(', def_start_line)
        if not fn_name_match:
            logging.warning(
                "Spurious Rust function definition line: %s", def_start_line)
            continue
        fn_name = fn_name_match.group(1)

        # get a complete function header and extract argument names
        func_header = def_start_line
        j = idx
        while '{' not in lines[j]:  # { will mark the start of a function body
            func_header += lines[j]
        argnames = [m.group(1) for m in re.finditer(r'(\w+)\s*:(?!:)',
                                                    func_header)]

        # extract documentation
        docstring_lines = []
        for j in range(idx - 1, 0, -1):
            line = lines[j].lstrip()
            if not line.startswith('///'):
                break
            # treat empty lines as paragraph separators
            line = line.lstrip('/').strip() or os.linesep
            # preserve bullet points by adding a newline before them
            line = re.sub(r'^\s*\*', os.linesep + '*', line)
            docstring_lines.append(line)
        docstring_lines.reverse()
        docstring = ''.join(docstring_lines)

        # fix the function data to account for some of the idiosyncrasies
        # that the expression functions exhibit
        fn_name = fn_name.rstrip('_')  # _ is used to avoid name collisions
        if argnames[:-1] == 'context':
            argnames = argnames[:-1]  # expression context is impl. detail

        # TODO: support some kind of docstring tags that'd describe
        # arguments and the return value
        func = Function(name=fn_name,
                        description=docstring,
                        arguments=OrderedDict(zip(argnames, repeat(None))),
                        returns=None)

        logging.debug(
            "Found function %s(%s) -> %s",
            func.name, ', '.join(func.arguments), func.returns or "?")
        functions.append(func)

    module = Module(path=path,
                    name=mod_name,
                    description=mod_docstring,
                    submodules=submodules,
                    functions=functions)

    logging.info("Module %s had %s function(s) and %s submodule(s)",
                 module.name, len(module.functions), len(module.functions))
    return module
