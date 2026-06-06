#include <unistd.h>
#ifdef link
#undef link
#endif
int (*foo)(const char *, const char *) = link;
int main(void) { return 0; }
