file(REMOVE_RECURSE
  "libduckdb_static.a"
  "libduckdb_static.pdb"
)

# Per-language clean rules from dependency scanning.
foreach(lang CXX)
  include(CMakeFiles/duckdb_static.dir/cmake_clean_${lang}.cmake OPTIONAL)
endforeach()
