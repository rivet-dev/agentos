file(REMOVE_RECURSE
  "../../lib/libc++abi.a"
  "../../lib/libc++abi.pdb"
)

# Per-language clean rules from dependency scanning.
foreach(lang ASM C CXX)
  include(CMakeFiles/cxxabi_static.dir/cmake_clean_${lang}.cmake OPTIONAL)
endforeach()
