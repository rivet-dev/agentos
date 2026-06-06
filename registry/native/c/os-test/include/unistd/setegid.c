#include <unistd.h>
#ifdef setegid
#undef setegid
#endif
int (*foo)(gid_t) = setegid;
int main(void) { return 0; }
