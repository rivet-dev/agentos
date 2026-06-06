#include <fcntl.h>
#ifndef F_GETFL
#error "F_GETFL is not defined"
#endif
int main(void) { return 0; }
