#include <unistd.h>
#ifdef setuid
#undef setuid
#endif
int (*foo)(uid_t) = setuid;
int main(void) { return 0; }
