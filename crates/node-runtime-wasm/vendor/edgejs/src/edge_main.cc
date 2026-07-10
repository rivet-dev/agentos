#include <iostream>
#include <string>

#include "edge_cli.h"

extern "C" char** uv_setup_args(int argc, char** argv);

int main(int argc, char** argv) {
  argv = uv_setup_args(argc, argv);
  EdgeInitializeCliProcess();
  std::string error;
  const int exit_code = EdgeRunCli(argc, const_cast<const char* const*>(argv), &error);
  if (!error.empty()) {
    std::cerr << error << "\n";
  }
  return exit_code;
}
