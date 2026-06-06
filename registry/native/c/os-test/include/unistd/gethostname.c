#include <unistd.h>
#ifdef gethostname
#undef gethostname
#endif
int (*foo)(char *, size_t) = gethostname;
int main(void) { return 0; }
