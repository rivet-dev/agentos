#include <fcntl.h>
#ifndef F_DUPFD_CLOFORK
#error "F_DUPFD_CLOFORK is not defined"
#endif
int main(void) { return 0; }
