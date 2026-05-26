#pragma once

#include <QImage>
#include <QObject>
#include <QString>
#include <expected>

#include "Error.hh"

namespace timelapse {

class ImageProcessor : public QObject {
   Q_OBJECT

public:
   explicit ImageProcessor(QObject* parent = nullptr);
   ~ImageProcessor() override = default;

   // Resize image with letterboxing (black bars to maintain aspect ratio)
   [[nodiscard]] auto resizeWithLetterbox(QImage const& source,
                                          int32_t targetWidth,
                                          int32_t targetHeight)
      -> std::expected<QImage, Error>;

   // Check if image is mostly black (screen off, screensaver, etc.)
   [[nodiscard]] auto isImageBlack(QImage const& image,
                                   double threshold = 0.01) const -> bool;

   // Save image to file as PNG
   [[nodiscard]] auto saveImage(QImage const& image, QString const& path)
      -> std::expected<void, Error>;

private:
   // Calculate mean brightness of image by sampling pixels
   [[nodiscard]] auto calculateMeanBrightness(QImage const& image) const -> double;
};

}  // namespace timelapse
