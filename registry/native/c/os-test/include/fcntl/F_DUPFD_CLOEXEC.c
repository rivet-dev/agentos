#include <fcntl.h>
#ifndef F_DUPFD_CLOEXEC
#error "F_DUPFD_CLOEXEC is not defined"
#endif
int main(void) { return 0; }
