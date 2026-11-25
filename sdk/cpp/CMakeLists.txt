cmake_minimum_required(VERSION 3.20)
project(lingxi_cpp_sdk LANGUAGES CXX VERSION 2.0.0)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

# Header-only SDK. Consumers link the Rust core themselves through the C ABI
# (see sdk/cpp/README.md); this package only guarantees the include tree.
add_library(lingxi_cpp_catalog INTERFACE)
target_include_directories(lingxi_cpp_catalog INTERFACE
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
    $<INSTALL_INTERFACE:include>
)
target_compile_features(lingxi_cpp_catalog INTERFACE cxx_std_20)

find_package(Threads REQUIRED)

add_executable(test_cpp_sdk tests/test_cpp_sdk.cpp)
target_include_directories(test_cpp_sdk PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/include
    ${CMAKE_CURRENT_SOURCE_DIR}/../../core/include
)
find_library(LINGXI_CORE_LIB lingxi_core PATHS
    ${CMAKE_CURRENT_SOURCE_DIR}/../../core/target/release
    ${CMAKE_CURRENT_SOURCE_DIR}/../../core/target/debug
    REQUIRED
)
target_link_libraries(test_cpp_sdk PRIVATE ${LINGXI_CORE_LIB} Threads::Threads)

enable_testing()
add_test(NAME test_cpp_sdk
    COMMAND test_cpp_sdk ${CMAKE_CURRENT_SOURCE_DIR}/../..
)

include(CMakePackageConfigHelpers)
configure_package_config_file(
    ${CMAKE_CURRENT_SOURCE_DIR}/lingxi_cpp_sdkConfig.cmake.in
    ${CMAKE_CURRENT_BINARY_DIR}/lingxi_cpp_sdkConfig.cmake
    INSTALL_DESTINATION lib/cmake/lingxi_cpp_sdk
)
write_basic_package_version_file(
    ${CMAKE_CURRENT_BINARY_DIR}/lingxi_cpp_sdkConfigVersion.cmake
    VERSION ${PROJECT_VERSION}
    COMPATIBILITY SameMajorVersion
)

install(DIRECTORY include/ DESTINATION include)
install(TARGETS lingxi_cpp_catalog EXPORT lingxi_cpp_sdkTargets)
install(EXPORT lingxi_cpp_sdkTargets
    FILE lingxi_cpp_sdkTargets.cmake
    NAMESPACE lingxi::
    DESTINATION lib/cmake/lingxi_cpp_sdk
)
install(FILES
    ${CMAKE_CURRENT_BINARY_DIR}/lingxi_cpp_sdkConfig.cmake
    ${CMAKE_CURRENT_BINARY_DIR}/lingxi_cpp_sdkConfigVersion.cmake
    DESTINATION lib/cmake/lingxi_cpp_sdk
)