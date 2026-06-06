#include <fcntl.h>
#ifndef F_UNLCK
#error "F_UNLCK is not defined"
#endif
int main(void) { return 0; }
