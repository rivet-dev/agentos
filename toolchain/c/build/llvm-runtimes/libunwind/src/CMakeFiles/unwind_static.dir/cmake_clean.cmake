file(REMOVE_RECURSE
  "../../lib/libunwind.a"
  "../../lib/libunwind.pdb"
)

# Per-language clean rules from dependency scanning.
foreach(lang ASM C CXX)
  include(CMakeFiles/unwind_static.dir/cmake_clean_${lang}.cmake OPTIONAL)
endforeach()
