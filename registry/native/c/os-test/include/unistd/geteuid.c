#include <unistd.h>
#ifdef geteuid
#undef geteuid
#endif
uid_t (*foo)(void) = geteuid;
int main(void) { return 0; }
