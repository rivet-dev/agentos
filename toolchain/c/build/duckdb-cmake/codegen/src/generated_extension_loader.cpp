#include "duckdb/main/extension/generated_extension_loader.hpp"
#include "duckdb/main/extension_helper.hpp"

namespace duckdb {

//! Looks through the CMake-generated list of extensions that are linked into DuckDB currently to try load <extension>
ExtensionLoadResult ExtensionHelper::LoadExtension(DuckDB &db, const std::string &extension) {
    if (extension=="core_functions") {
        db.LoadStaticExtension<CoreFunctionsExtension>();
        return ExtensionLoadResult::LOADED_EXTENSION;
    }

    return ExtensionLoadResult::NOT_LOADED;
}

vector<string> LinkedExtensions(){
    vector<string> VEC = {
	"core_functions"
    };
    return VEC;
}

void ExtensionHelper::LoadAllExtensions(DuckDB &db) {
    for (auto& ext_name : LinkedExtensions()) {
        LoadExtension(db, ext_name);
    }
}

vector<string> ExtensionHelper::LoadedExtensionTestPaths(){
    vector<string> VEC = {
    };
    return VEC;
}
}
