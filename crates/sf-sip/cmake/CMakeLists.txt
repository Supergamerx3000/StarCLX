# Baut nur die C-Schicht src/shim.c mit denselben Definitionen wie libre.
cmake_minimum_required(VERSION 3.18)
project(sfsip C)

find_package(re CONFIG REQUIRED)
add_compile_definitions(${RE_DEFINITIONS})

if(MSVC)
  # wie libre selbst: MSVC kennt sonst kein C11
  add_compile_options(-std:c11)
endif()

add_library(sfsip STATIC ${SFSIP_SHIM})
target_include_directories(sfsip PRIVATE ${RE_INCLUDE} ${BARESIP_INCLUDE})
set_target_properties(sfsip PROPERTIES POSITION_INDEPENDENT_CODE ON)
install(TARGETS sfsip ARCHIVE DESTINATION lib)
