cmake_minimum_required(VERSION 3.20)
project(IntervoxDriver C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_OSX_DEPLOYMENT_TARGET "14.0" CACHE STRING "" FORCE)

add_library(Intervox MODULE src/Intervox.c)

target_include_directories(Intervox PRIVATE ${CMAKE_SOURCE_DIR}/include)
target_compile_options(Intervox PRIVATE -Wall -Wextra)

# Produce Intervox.driver/Contents/{MacOS/Intervox,Info.plist}
set_target_properties(Intervox PROPERTIES
    BUNDLE TRUE
    BUNDLE_EXTENSION "driver"
    OUTPUT_NAME "Intervox"
    PREFIX ""
    SUFFIX ""
    MACOSX_BUNDLE_INFO_PLIST ${CMAKE_SOURCE_DIR}/Info.plist)

target_link_libraries(Intervox PRIVATE
    "-framework CoreAudio"
    "-framework CoreFoundation")

enable_testing()

add_executable(driver_property_contract tests/property_contract.c)
target_include_directories(driver_property_contract PRIVATE ${CMAKE_SOURCE_DIR}/include)
target_compile_options(driver_property_contract PRIVATE -Wall -Wextra)
target_link_libraries(driver_property_contract PRIVATE
    "-framework CoreAudio"
    "-framework CoreFoundation")
add_test(NAME driver_property_contract COMMAND driver_property_contract)
