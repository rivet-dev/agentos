#include <sys/resource.h>
#ifdef setrlimit
#undef setrlimit
#endif
int (*foo)(int, const struct rlimit *) = setrlimit;
int main(void) { return 0; }
