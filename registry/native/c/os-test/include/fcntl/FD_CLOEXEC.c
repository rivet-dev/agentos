/*[SPN]*/
#include <fcntl.h>
#ifndef FD_CLOEXEC
#error "FD_CLOEXEC is not defined"
#endif
int main(void) { return 0; }
