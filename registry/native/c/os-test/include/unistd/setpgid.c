#include <unistd.h>
#ifdef setpgid
#undef setpgid
#endif
int (*foo)(pid_t, pid_t) = setpgid;
int main(void) { return 0; }
