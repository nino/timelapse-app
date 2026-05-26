#pragma once

#include <QSlider>
#include <QWidget>

namespace timelapse {

class FrameScrubber : public QWidget {
   Q_OBJECT
   Q_PROPERTY(int value READ value WRITE setValue NOTIFY valueChanged)
   Q_PROPERTY(int minimum READ minimum WRITE setMinimum)
   Q_PROPERTY(int maximum READ maximum WRITE setMaximum)
   Q_PROPERTY(bool enabled READ isEnabled WRITE setEnabled)

public:
   explicit FrameScrubber(QWidget* parent = nullptr);
   ~FrameScrubber() override = default;

   [[nodiscard]] auto value() const -> int;
   [[nodiscard]] auto minimum() const -> int;
   [[nodiscard]] auto maximum() const -> int;

public slots:
   void setValue(int value);
   void setMinimum(int min);
   void setMaximum(int max);
   void setRange(int min, int max);
   void setEnabled(bool enabled);

signals:
   void valueChanged(int value);
   void sliderPressed();
   void sliderReleased();

private:
   void setupStylesheet();

   QSlider* _slider{nullptr};
};

}  // namespace timelapse
