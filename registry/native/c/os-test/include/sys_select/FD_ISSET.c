#include <sys/select.h>
#ifndef FD_ISSET
int (*foo)(int, const fd_set *) = FD_ISSET;
#endif
int main(void) { return 0; }
