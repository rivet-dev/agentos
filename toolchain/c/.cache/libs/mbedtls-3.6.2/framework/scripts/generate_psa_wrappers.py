#!/usr/bin/env python3
"""Generate wrapper functions for PSA function calls.
"""

# Copyright The Mbed TLS Contributors
# SPDX-License-Identifier: Apache-2.0 OR GPL-2.0-or-later

import argparse
from mbedtls_framework.code_wrapper.psa_test_wrapper import PSATestWrapper, PSALoggingTestWrapper

DEFAULT_C_OUTPUT_FILE_NAME = 'tests/src/psa_test_wrappers.c'
DEFAULT_H_OUTPUT_FILE_NAME = 'tests/include/test/psa_test_wrappers.h'

def main() -> None:
    parser = argparse.ArgumentParser(description=globals()['__doc__'])
    parser.add_argument('--log',
                        help='Stream to log to (default: no logging code)')
    parser.add_argument('--output-c',
                        metavar='FILENAME',
                        default=DEFAULT_C_OUTPUT_FILE_NAME,
                        help=('Output .c file path (default: {}; skip .c output if empty)'
                              .format(DEFAULT_C_OUTPUT_FILE_NAME)))
    parser.add_argument('--output-h',
                        metavar='FILENAME',
                        default=DEFAULT_H_OUTPUT_FILE_NAME,
                        help=('Output .h file path (default: {}; skip .h output if empty)'
                              .format(DEFAULT_H_OUTPUT_FILE_NAME)))
    options = parser.parse_args()
    if options.log:
        generator = PSALoggingTestWrapper(DEFAULT_H_OUTPUT_FILE_NAME,
                                          DEFAULT_C_OUTPUT_FILE_NAME,
                                          options.log) #type: PSATestWrapper
    else:
        generator = PSATestWrapper(DEFAULT_H_OUTPUT_FILE_NAME,
                                   DEFAULT_C_OUTPUT_FILE_NAME)

    if options.output_h:
        generator.write_h_file(options.output_h)
    if options.output_c:
        generator.write_c_file(options.output_c)

if __name__ == '__main__':
    main()
