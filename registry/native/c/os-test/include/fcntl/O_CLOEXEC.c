#include <fcntl.h>
#ifndef O_CLOEXEC
#error "O_CLOEXEC is not defined"
#endif
int main(void) { return 0; }
