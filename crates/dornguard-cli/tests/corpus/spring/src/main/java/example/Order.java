package example;

import jakarta.persistence.Entity;
import jakarta.persistence.FetchType;
import jakarta.persistence.Id;
import jakarta.persistence.ManyToOne;
import jakarta.persistence.OneToMany;
import java.util.List;
import lombok.Data;
import lombok.EqualsAndHashCode;
import lombok.Getter;
import lombok.Setter;

@Data
@Entity
class Order { // expect: spring.jpa-lombok-entity
    @Id
    private Long id;

    @ManyToOne(fetch = FetchType.EAGER) // expect: spring.jpa-eager-fetch
    private Customer customer;

    @OneToMany(mappedBy = "order", fetch = FetchType.LAZY)
    private List<OrderLine> lines;
}

@Entity
@EqualsAndHashCode
class Customer { // expect: spring.jpa-lombok-entity
    @Id
    private Long id;
}

// Getters, setters and identifier-based equality are the safe forms.
@Entity
@Getter
@Setter
@EqualsAndHashCode(onlyExplicitlyIncluded = true)
class OrderLine {
    @Id
    @EqualsAndHashCode.Include
    private Long id;

    @ManyToOne
    private Order order;
}

// Lombok on a class that is not an entity is not reported.
@Data
class OrderView {
    private Long id;
}
