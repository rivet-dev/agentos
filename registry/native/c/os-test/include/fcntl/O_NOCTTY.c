#include <fcntl.h>
#ifndef O_NOCTTY
#error "O_NOCTTY is not defined"
#endif
int main(void) { return 0; }
