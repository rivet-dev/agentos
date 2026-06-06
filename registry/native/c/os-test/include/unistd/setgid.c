#include <unistd.h>
#ifdef setgid
#undef setgid
#endif
int (*foo)(gid_t) = setgid;
int main(void) { return 0; }
