#include <stdint.h>
#include "./lhl.h"

struct Node {
        struct Node const *next;
        int value;
};

struct Node *node_new(int value, struct Node const *next)
{
        struct Node *this = (struct Node *)lhlmalloc(sizeof(struct Node), _Alignof(struct Node));
        this->next = next;
        this->value = value;
        return this;
}



int main()
{
        struct Node *c = node_new(0xffffffffu, NULL);
        struct Node *b = node_new(0x42424242u, c);
        struct Node *a = node_new(0, b);
        lhl_graph_from_root(a, sizeof(uintptr_t), "pure-c.json");
        return 0;
}