#include <sys/select.h>
#ifndef FD_CLR
void (*foo)(int, fd_set *) = FD_CLR;
#endif
int main(void) { return 0; }
