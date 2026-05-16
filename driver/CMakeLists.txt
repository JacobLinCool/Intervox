cmake_minimum_required(VERSION 3.20)
project(IntervoxDriver C)

set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_OSX_DEPLOYMENT_TARGET "12.0" CACHE STRING "")

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
