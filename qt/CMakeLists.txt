cmake_minimum_required(VERSION 3.25)

project(timelapse-qt
   VERSION 1.0.0
   LANGUAGES CXX
)

# Require C++23
set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# Qt6 configuration
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)
set(CMAKE_AUTOUIC ON)

find_package(Qt6 REQUIRED COMPONENTS
   Core
   Gui
   Widgets
   Sql
   Concurrent
)

# Platform-specific dependencies
if(APPLE)
   find_library(COCOA_LIBRARY Cocoa REQUIRED)
   find_library(COREGRAPHICS_LIBRARY CoreGraphics REQUIRED)
   find_library(APPSERVICES_LIBRARY ApplicationServices REQUIRED)
elseif(UNIX AND NOT APPLE)
   find_package(X11 REQUIRED)
endif()

# Source files
set(CORE_SOURCES
   src/core/ErrorLog.cc
   src/core/Database.cc
   src/core/ImageProcessor.cc
   src/core/ScreenCapture.cc
   src/core/Photographer.cc
   src/core/VideoExtractor.cc
   src/core/CacheManager.cc
)

set(CORE_HEADERS
   src/core/ErrorLog.hh
   src/core/Database.hh
   src/core/ImageProcessor.hh
   src/core/ScreenCapture.hh
   src/core/Photographer.hh
   src/core/VideoExtractor.hh
   src/core/CacheManager.hh
)

set(MODEL_SOURCES
   src/models/FolderListModel.cc
   src/models/FileListModel.cc
   src/models/VideoListModel.cc
)

set(MODEL_HEADERS
   src/models/FolderListModel.hh
   src/models/FileListModel.hh
   src/models/VideoListModel.hh
)

set(UI_SOURCES
   src/ui/MainWindow.cc
   src/ui/ImageViewer.cc
   src/ui/FrameScrubber.cc
)

set(UI_HEADERS
   src/ui/MainWindow.hh
   src/ui/ImageViewer.hh
   src/ui/FrameScrubber.hh
)

set(UTIL_SOURCES
   src/utils/PathUtils.cc
)

set(UTIL_HEADERS
   src/utils/Constants.hh
   src/utils/PathUtils.hh
)

# Platform-specific screen capture implementations
if(APPLE)
   list(APPEND CORE_SOURCES src/core/ScreenCaptureMac.cc)
elseif(UNIX AND NOT APPLE)
   list(APPEND CORE_SOURCES src/core/ScreenCaptureLinux.cc)
elseif(WIN32)
   list(APPEND CORE_SOURCES src/core/ScreenCaptureWindows.cc)
endif()

# Resources
set(RESOURCES
   resources/resources.qrc
)

# Main executable
add_executable(timelapse-qt
   src/main.cc
   ${CORE_SOURCES}
   ${CORE_HEADERS}
   ${MODEL_SOURCES}
   ${MODEL_HEADERS}
   ${UI_SOURCES}
   ${UI_HEADERS}
   ${UTIL_SOURCES}
   ${UTIL_HEADERS}
   ${RESOURCES}
)

target_include_directories(timelapse-qt PRIVATE
   ${CMAKE_CURRENT_SOURCE_DIR}/src
)

target_link_libraries(timelapse-qt PRIVATE
   Qt6::Core
   Qt6::Gui
   Qt6::Widgets
   Qt6::Sql
   Qt6::Concurrent
)

# Platform-specific linking
if(APPLE)
   target_link_libraries(timelapse-qt PRIVATE
      ${COCOA_LIBRARY}
      ${COREGRAPHICS_LIBRARY}
      ${APPSERVICES_LIBRARY}
   )
elseif(UNIX AND NOT APPLE)
   target_link_libraries(timelapse-qt PRIVATE
      X11::X11
   )
endif()

# Set output directory
set_target_properties(timelapse-qt PROPERTIES
   RUNTIME_OUTPUT_DIRECTORY ${CMAKE_BINARY_DIR}/bin
)

# macOS bundle settings
if(APPLE)
   set_target_properties(timelapse-qt PROPERTIES
      MACOSX_BUNDLE TRUE
      MACOSX_BUNDLE_BUNDLE_NAME "Timelapse"
      MACOSX_BUNDLE_BUNDLE_VERSION ${PROJECT_VERSION}
      MACOSX_BUNDLE_SHORT_VERSION_STRING ${PROJECT_VERSION}
      MACOSX_BUNDLE_GUI_IDENTIFIER "com.timelapse.qt"
   )
endif()
