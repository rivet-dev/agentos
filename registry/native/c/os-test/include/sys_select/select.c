#include <sys/select.h>
#ifdef select
#undef select
#endif
int (*foo)(int, fd_set *restrict, fd_set *restrict, fd_set *restrict, struct timeval *restrict) = select;
int main(void) { return 0; }
