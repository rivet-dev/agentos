#include <sys/select.h>
#ifdef pselect
#undef pselect
#endif
int (*foo)(int, fd_set *restrict, fd_set *restrict, fd_set *restrict, const struct timespec *restrict, const sigset_t *restrict) = pselect;
int main(void) { return 0; }
