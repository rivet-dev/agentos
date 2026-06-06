#include <unistd.h>
#ifdef execle
#undef execle
#endif
int (*foo)(const char *, const char *, ...) = execle;
int main(void) { return 0; }
