#include <sys/resource.h>
#ifdef getrlimit
#undef getrlimit
#endif
int (*foo)(int, struct rlimit *) = getrlimit;
int main(void) { return 0; }
