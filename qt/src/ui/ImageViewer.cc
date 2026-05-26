#include "ImageViewer.hh"

#include <QImageReader>
#include <QPainter>
#include <QResizeEvent>

namespace timelapse {

ImageViewer::ImageViewer(QWidget* parent)
   : QWidget(parent) {
   setMinimumSize(200, 150);
   setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Expanding);
   setAutoFillBackground(true);

   // Set dark background
   QPalette pal = palette();
   pal.setColor(QPalette::Window, QColor(30, 30, 30));
   setPalette(pal);
}

void ImageViewer::setImage(QImage const& image) {
   if (image.isNull()) {
      this->clear();
      return;
   }

   this->_pixmap = QPixmap::fromImage(image);
   this->updateScaledPixmap();
   update();
}

void ImageViewer::setImagePath(QString const& path) {
   if (path.isEmpty()) {
      this->clear();
      return;
   }

   QImageReader reader(path);
   reader.setAutoTransform(true);

   QImage image = reader.read();
   if (image.isNull()) {
      emit imageLoadError("Failed to load image: " + reader.errorString());
      return;
   }

   this->setImage(image);
   emit imageLoaded(path);
}

void ImageViewer::clear() {
   this->_pixmap = QPixmap();
   this->_scaledPixmap = QPixmap();
   update();
}

auto ImageViewer::hasImage() const -> bool {
   return !this->_pixmap.isNull();
}

void ImageViewer::setPlaceholderText(QString const& text) {
   this->_placeholderText = text;
   if (!this->hasImage()) {
      update();
   }
}

void ImageViewer::paintEvent(QPaintEvent* event) {
   Q_UNUSED(event)

   QPainter painter(this);
   painter.setRenderHint(QPainter::Antialiasing);
   painter.setRenderHint(QPainter::SmoothPixmapTransform);

   if (this->_scaledPixmap.isNull()) {
      // Draw placeholder text
      painter.setPen(QColor(128, 128, 128));
      painter.setFont(QFont("sans-serif", 16));
      painter.drawText(rect(), Qt::AlignCenter, this->_placeholderText);
      return;
   }

   // Calculate position to center the image
   int x = (width() - this->_scaledPixmap.width()) / 2;
   int y = (height() - this->_scaledPixmap.height()) / 2;

   painter.drawPixmap(x, y, this->_scaledPixmap);
}

void ImageViewer::resizeEvent(QResizeEvent* event) {
   QWidget::resizeEvent(event);
   this->updateScaledPixmap();
}

void ImageViewer::updateScaledPixmap() {
   if (this->_pixmap.isNull()) {
      this->_scaledPixmap = QPixmap();
      return;
   }

   // Scale to fit while maintaining aspect ratio
   this->_scaledPixmap = this->_pixmap.scaled(
      size(),
      Qt::KeepAspectRatio,
      Qt::SmoothTransformation);
}

}  // namespace timelapse
