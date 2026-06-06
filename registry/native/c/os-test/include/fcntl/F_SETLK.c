#include <fcntl.h>
#ifndef F_SETLK
#error "F_SETLK is not defined"
#endif
int main(void) { return 0; }
