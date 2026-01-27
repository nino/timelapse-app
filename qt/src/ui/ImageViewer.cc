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
      clear();
      return;
   }

   m_pixmap = QPixmap::fromImage(image);
   updateScaledPixmap();
   update();
}

void ImageViewer::setImagePath(QString const& path) {
   if (path.isEmpty()) {
      clear();
      return;
   }

   QImageReader reader(path);
   reader.setAutoTransform(true);

   QImage image = reader.read();
   if (image.isNull()) {
      emit imageLoadError("Failed to load image: " + reader.errorString());
      return;
   }

   setImage(image);
   emit imageLoaded(path);
}

void ImageViewer::clear() {
   m_pixmap = QPixmap();
   m_scaledPixmap = QPixmap();
   update();
}

auto ImageViewer::hasImage() const -> bool {
   return !m_pixmap.isNull();
}

void ImageViewer::setPlaceholderText(QString const& text) {
   m_placeholderText = text;
   if (!hasImage()) {
      update();
   }
}

void ImageViewer::paintEvent(QPaintEvent* event) {
   Q_UNUSED(event)

   QPainter painter(this);
   painter.setRenderHint(QPainter::Antialiasing);
   painter.setRenderHint(QPainter::SmoothPixmapTransform);

   if (m_scaledPixmap.isNull()) {
      // Draw placeholder text
      painter.setPen(QColor(128, 128, 128));
      painter.setFont(QFont("sans-serif", 16));
      painter.drawText(rect(), Qt::AlignCenter, m_placeholderText);
      return;
   }

   // Calculate position to center the image
   int x = (width() - m_scaledPixmap.width()) / 2;
   int y = (height() - m_scaledPixmap.height()) / 2;

   painter.drawPixmap(x, y, m_scaledPixmap);
}

void ImageViewer::resizeEvent(QResizeEvent* event) {
   QWidget::resizeEvent(event);
   updateScaledPixmap();
}

void ImageViewer::updateScaledPixmap() {
   if (m_pixmap.isNull()) {
      m_scaledPixmap = QPixmap();
      return;
   }

   // Scale to fit while maintaining aspect ratio
   m_scaledPixmap = m_pixmap.scaled(
      size(),
      Qt::KeepAspectRatio,
      Qt::SmoothTransformation);
}

}  // namespace timelapse
