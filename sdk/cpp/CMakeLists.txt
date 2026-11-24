cmake_minimum_required(VERSION 3.20)
project(lingxi_cpp_sdk LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

add_library(lingxi_cpp_catalog INTERFACE)
target_include_directories(lingxi_cpp_catalog INTERFACE ${CMAKE_CURRENT_SOURCE_DIR}/include)

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
target_link_libraries(test_cpp_sdk PRIVATE ${LINGXI_CORE_LIB})
