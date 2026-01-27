#include "ImageProcessor.hh"

#include <QColor>
#include <QPainter>

#include "utils/Constants.hh"

namespace timelapse {

ImageProcessor::ImageProcessor(QObject* parent)
   : QObject(parent) {}

auto ImageProcessor::resizeWithLetterbox(QImage const& source,
                                         int32_t targetWidth,
                                         int32_t targetHeight)
   -> std::expected<QImage, QString> {
   if (source.isNull()) {
      return std::unexpected("Source image is null");
   }

   // Create a black background image at target size
   QImage result(targetWidth, targetHeight, QImage::Format_RGB32);
   result.fill(Qt::black);

   // Calculate scaling to fit within target while maintaining aspect ratio
   double sourceRatio = static_cast<double>(source.width()) / source.height();
   double targetRatio = static_cast<double>(targetWidth) / targetHeight;

   int32_t scaledWidth;
   int32_t scaledHeight;

   if (sourceRatio > targetRatio) {
      // Source is wider - fit to width
      scaledWidth = targetWidth;
      scaledHeight = static_cast<int32_t>(targetWidth / sourceRatio);
   } else {
      // Source is taller - fit to height
      scaledHeight = targetHeight;
      scaledWidth = static_cast<int32_t>(targetHeight * sourceRatio);
   }

   // Scale the source image
   QImage scaled = source.scaled(scaledWidth, scaledHeight,
                                 Qt::KeepAspectRatio,
                                 Qt::SmoothTransformation);

   // Calculate position to center the scaled image
   int32_t x = (targetWidth - scaledWidth) / 2;
   int32_t y = (targetHeight - scaledHeight) / 2;

   // Draw the scaled image onto the black background
   QPainter painter(&result);
   painter.drawImage(x, y, scaled);
   painter.end();

   return result;
}

auto ImageProcessor::isImageBlack(QImage const& image, double threshold) const -> bool {
   double brightness = calculateMeanBrightness(image);
   return brightness < threshold;
}

auto ImageProcessor::saveImage(QImage const& image, QString const& path)
   -> std::expected<void, QString> {
   if (image.isNull()) {
      return std::unexpected("Cannot save null image");
   }

   if (!image.save(path, "PNG")) {
      return std::unexpected("Failed to save image to: " + path);
   }

   return {};
}

auto ImageProcessor::calculateMeanBrightness(QImage const& image) const -> double {
   if (image.isNull()) {
      return 0.0;
   }

   // Convert to format that allows easy pixel access
   QImage converted = image.convertToFormat(QImage::Format_RGB32);

   double totalBrightness = 0.0;
   int32_t sampleCount = 0;

   // Sample pixels at intervals for performance
   for (int32_t y = 0; y < converted.height(); y += kSampleStep) {
      for (int32_t x = 0; x < converted.width(); x += kSampleStep) {
         QColor pixel(converted.pixel(x, y));

         // Calculate luminance using standard weights
         double luminance = 0.299 * pixel.red() +
                           0.587 * pixel.green() +
                           0.114 * pixel.blue();

         totalBrightness += luminance / 255.0;
         sampleCount++;
      }
   }

   if (sampleCount == 0) {
      return 0.0;
   }

   return totalBrightness / sampleCount;
}

}  // namespace timelapse
