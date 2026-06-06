#include <fcntl.h>
#ifndef F_GETLK
#error "F_GETLK is not defined"
#endif
int main(void) { return 0; }
