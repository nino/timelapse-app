#include "FrameScrubber.hh"

#include <QHBoxLayout>

namespace timelapse {

FrameScrubber::FrameScrubber(QWidget* parent)
   : QWidget(parent)
   , _slider(new QSlider(Qt::Horizontal, this)) {
   auto* layout = new QHBoxLayout(this);
   layout->setContentsMargins(0, 0, 0, 0);
   layout->addWidget(this->_slider);

   this->_slider->setMinimum(0);
   this->_slider->setMaximum(0);
   this->_slider->setValue(0);

   connect(this->_slider, &QSlider::valueChanged, this, &FrameScrubber::valueChanged);
   connect(this->_slider, &QSlider::sliderPressed, this, &FrameScrubber::sliderPressed);
   connect(this->_slider, &QSlider::sliderReleased, this, &FrameScrubber::sliderReleased);

   this->setupStylesheet();
}

auto FrameScrubber::value() const -> int {
   return this->_slider->value();
}

auto FrameScrubber::minimum() const -> int {
   return this->_slider->minimum();
}

auto FrameScrubber::maximum() const -> int {
   return this->_slider->maximum();
}

void FrameScrubber::setValue(int value) {
   this->_slider->setValue(value);
}

void FrameScrubber::setMinimum(int min) {
   this->_slider->setMinimum(min);
}

void FrameScrubber::setMaximum(int max) {
   this->_slider->setMaximum(max);
}

void FrameScrubber::setRange(int min, int max) {
   this->_slider->setRange(min, max);
}

void FrameScrubber::setEnabled(bool enabled) {
   this->_slider->setEnabled(enabled);
   QWidget::setEnabled(enabled);
}

void FrameScrubber::setupStylesheet() {
   this->_slider->setStyleSheet(R"(
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
