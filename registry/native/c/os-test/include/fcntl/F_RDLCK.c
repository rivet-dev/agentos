#include <fcntl.h>
#ifndef F_RDLCK
#error "F_RDLCK is not defined"
#endif
int main(void) { return 0; }
