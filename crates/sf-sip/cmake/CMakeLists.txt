# Baut nur die C-Schicht src/shim.c mit denselben Definitionen wie libre.
cmake_minimum_required(VERSION 3.18)
project(sfsip C)

find_package(re CONFIG REQUIRED)
find_package(OpenSSL REQUIRED)
add_compile_definitions(${RE_DEFINITIONS})

add_library(sfsip STATIC ${SFSIP_SHIM})
target_include_directories(sfsip PRIVATE ${RE_INCLUDE} ${BARESIP_INCLUDE} ${OPENSSL_INCLUDE_DIR})
set_target_properties(sfsip PROPERTIES POSITION_INDEPENDENT_CODE ON)
install(TARGETS sfsip ARCHIVE DESTINATION lib)
