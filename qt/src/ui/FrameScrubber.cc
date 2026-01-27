#include "FrameScrubber.hh"

#include <QHBoxLayout>

namespace timelapse {

FrameScrubber::FrameScrubber(QWidget* parent)
   : QWidget(parent)
   , m_slider(new QSlider(Qt::Horizontal, this)) {
   auto* layout = new QHBoxLayout(this);
   layout->setContentsMargins(0, 0, 0, 0);
   layout->addWidget(m_slider);

   m_slider->setMinimum(0);
   m_slider->setMaximum(0);
   m_slider->setValue(0);

   connect(m_slider, &QSlider::valueChanged, this, &FrameScrubber::valueChanged);
   connect(m_slider, &QSlider::sliderPressed, this, &FrameScrubber::sliderPressed);
   connect(m_slider, &QSlider::sliderReleased, this, &FrameScrubber::sliderReleased);

   setupStylesheet();
}

auto FrameScrubber::value() const -> int {
   return m_slider->value();
}

auto FrameScrubber::minimum() const -> int {
   return m_slider->minimum();
}

auto FrameScrubber::maximum() const -> int {
   return m_slider->maximum();
}

void FrameScrubber::setValue(int value) {
   m_slider->setValue(value);
}

void FrameScrubber::setMinimum(int min) {
   m_slider->setMinimum(min);
}

void FrameScrubber::setMaximum(int max) {
   m_slider->setMaximum(max);
}

void FrameScrubber::setRange(int min, int max) {
   m_slider->setRange(min, max);
}

void FrameScrubber::setEnabled(bool enabled) {
   m_slider->setEnabled(enabled);
   QWidget::setEnabled(enabled);
}

void FrameScrubber::setupStylesheet() {
   m_slider->setStyleSheet(R"(
      QSlider::groove:horizontal {
         border: 1px solid #4b5563;
         height: 8px;
         background: #4b5563;
         margin: 2px 0;
         border-radius: 4px;
      }

      QSlider::handle:horizontal {
         background: #3b82f6;
         border: 1px solid #2563eb;
         width: 18px;
         margin: -5px 0;
         border-radius: 9px;
      }

      QSlider::handle:horizontal:hover {
         background: #60a5fa;
      }

      QSlider::handle:horizontal:pressed {
         background: #1d4ed8;
      }

      QSlider::handle:horizontal:disabled {
         background: #6b7280;
         border: 1px solid #4b5563;
      }

      QSlider::groove:horizontal:disabled {
         background: #374151;
      }
   )");
}

}  // namespace timelapse
