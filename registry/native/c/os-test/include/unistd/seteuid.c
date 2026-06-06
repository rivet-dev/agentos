#include <unistd.h>
#ifdef seteuid
#undef seteuid
#endif
int (*foo)(uid_t) = seteuid;
int main(void) { return 0; }
