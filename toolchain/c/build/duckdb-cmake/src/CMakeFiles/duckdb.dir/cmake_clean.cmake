file(REMOVE_RECURSE
  "libduckdb.pdb"
  "libduckdb.so"
)

# Per-language clean rules from dependency scanning.
foreach(lang CXX)
  include(CMakeFiles/duckdb.dir/cmake_clean_${lang}.cmake OPTIONAL)
endforeach()
