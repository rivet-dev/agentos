#include <unistd.h>
#ifdef execv
#undef execv
#endif
int (*foo)(const char *, char *const []) = execv;
int main(void) { return 0; }
