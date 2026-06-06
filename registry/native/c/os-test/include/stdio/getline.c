#include <stdio.h>
#ifdef getline
#undef getline
#endif
ssize_t (*foo)(char **restrict, size_t *restrict, FILE *restrict) = getline;
int main(void) { return 0; }
