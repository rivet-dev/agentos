#include <unistd.h>
#ifdef execl
#undef execl
#endif
int (*foo)(const char *, const char *, ...) = execl;
int main(void) { return 0; }
