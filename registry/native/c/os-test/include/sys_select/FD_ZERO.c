#include <sys/select.h>
#ifndef FD_ZERO
void (*foo)(fd_set *) = FD_ZERO;
#endif
int main(void) { return 0; }
