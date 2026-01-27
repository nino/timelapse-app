#pragma once

#include <QImage>
#include <QPixmap>
#include <QWidget>

namespace timelapse {

class ImageViewer : public QWidget {
   Q_OBJECT

public:
   explicit ImageViewer(QWidget* parent = nullptr);
   ~ImageViewer() override = default;

   // Set image directly
   void setImage(QImage const& image);

   // Load image from file path
   void setImagePath(QString const& path);

   // Clear the displayed image
   void clear();

   // Check if an image is loaded
   [[nodiscard]] auto hasImage() const -> bool;

   // Set placeholder text shown when no image is loaded
   void setPlaceholderText(QString const& text);

signals:
   void imageLoaded(QString const& path);
   void imageLoadError(QString const& error);

protected:
   void paintEvent(QPaintEvent* event) override;
   void resizeEvent(QResizeEvent* event) override;

private:
   void updateScaledPixmap();

   QPixmap _pixmap;
   QPixmap _scaledPixmap;
   QString _placeholderText{"No image loaded"};
};

}  // namespace timelapse
