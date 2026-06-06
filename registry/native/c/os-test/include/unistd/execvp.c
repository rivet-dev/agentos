#include <unistd.h>
#ifdef execvp
#undef execvp
#endif
int (*foo)(const char *, char *const []) = execvp;
int main(void) { return 0; }
