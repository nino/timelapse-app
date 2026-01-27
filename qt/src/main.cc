#include <QApplication>

#include "ui/MainWindow.hh"

int main(int argc, char* argv[]) {
   QApplication app(argc, argv);

   app.setApplicationName("Timelapse");
   app.setApplicationVersion("1.0.0");
   app.setOrganizationName("Timelapse");

   // Set application-wide style
   app.setStyle("Fusion");

   timelapse::MainWindow mainWindow;
   mainWindow.show();

   return app.exec();
}
